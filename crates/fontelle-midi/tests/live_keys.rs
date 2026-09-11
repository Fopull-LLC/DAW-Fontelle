//! Which keys are down *right now*, for the window to draw (TDD §14.1).
//!
//! Asked for from playing the studio: *"when midi keys are pressed and
//! triggered it should highlight the note of the piano in the piano roll that
//! is being played"* — so that a phrase played on a controller can be seen on
//! the keyboard down the side of the roll and then written in.
//!
//! # Why a shared cell, and not a channel
//!
//! The same argument as [`LiveTarget`] and [`LiveMapping`], from the other
//! end: a device callback runs on the driver's own thread and may not block,
//! and the window wakes on its own schedule. What the window needs is not
//! every event that ever happened — it is *what is down at the moment the
//! frame is drawn*, which is a value, not a stream. So the router mirrors its
//! own held set into a pair of atomics the UI thread loads once a frame.
//!
//! # What is lit is what is sounding
//!
//! Not what arrived on the wire. Transpose is applied on the way in, so the
//! key that lights is the key the instrument is playing — otherwise the
//! highlight and the note you would draw from it disagree by the transpose.
//! A note the pedal is holding is still sounding, so it stays lit until the
//! pedal lets it go, and a device that is unplugged mid-chord takes its own
//! lights out with its notes.
//!
//! [`LiveTarget`]: fontelle_midi::LiveTarget
//! [`LiveMapping`]: fontelle_midi::LiveMapping

use std::sync::Arc;

use fontelle_midi::{DeviceMapping, InputSettings, LiveKeys, LiveMapping, MidiRouter};
use fontelle_types::{EventSink, NodeId, TimedEvent};

#[derive(Default)]
struct Sink;

impl EventSink for Sink {
    fn send(&mut self, _event: TimedEvent) -> bool {
        true
    }
}

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;
const CC: u8 = 0xb0;
const SUSTAIN: u8 = 64;

fn node() -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(7))
}

fn rig() -> (MidiRouter, Arc<LiveKeys>, Sink) {
    let lit = Arc::new(LiveKeys::default());
    let router =
        MidiRouter::new(node(), 0, DeviceMapping::default()).watching_keys(Arc::clone(&lit));
    (router, lit, Sink)
}

#[test]
fn nothing_is_lit_until_something_is_played() {
    let (_r, lit, _out) = rig();
    assert_eq!(lit.snapshot(), 0);
    assert!(!lit.is_down(60));
}

#[test]
fn a_key_that_is_down_is_lit_and_a_key_that_is_up_is_not() {
    let (mut r, lit, mut out) = rig();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    assert!(lit.is_down(60), "the key being played must be lit");
    assert!(!lit.is_down(61), "and only that key");

    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(lit.snapshot(), 0, "letting go must put the light out");
}

#[test]
fn a_chord_lights_every_key_in_it() {
    let (mut r, lit, mut out) = rig();
    for key in [60, 64, 67] {
        r.handle(&[NOTE_ON, key, 100], &mut out);
    }
    assert_eq!(
        lit.snapshot(),
        (1u128 << 60) | (1u128 << 64) | (1u128 << 67)
    );
    r.handle(&[NOTE_OFF, 64, 0], &mut out);
    assert_eq!(lit.snapshot(), (1u128 << 60) | (1u128 << 67));
}

#[test]
fn the_key_that_lights_is_the_key_that_sounds() {
    // Transpose is applied on the way in. Lighting the key the wire named
    // would put the highlight a whole tone from the note it plays, which is
    // exactly the mistake the highlight exists to prevent.
    let settings = InputSettings {
        transpose_semitones: 2,
        ..InputSettings::default()
    };
    let lit = Arc::new(LiveKeys::default());
    let mut r = MidiRouter::new(node(), 0, DeviceMapping::default())
        .watching_keys(Arc::clone(&lit))
        .following_input(Arc::new(LiveMapping::new(settings)));
    let mut out = Sink;

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    assert!(lit.is_down(62), "the sounding key is the lit one");
    assert!(!lit.is_down(60));
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(lit.snapshot(), 0, "and it goes out on the same mapping");
}

#[test]
fn a_note_the_pedal_is_holding_stays_lit_until_the_pedal_lets_go() {
    let (mut r, lit, mut out) = rig();
    r.handle(&[CC, SUSTAIN, 127], &mut out);
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert!(
        lit.is_down(60),
        "the key is up but the note is still sounding"
    );

    r.handle(&[CC, SUSTAIN, 0], &mut out);
    assert_eq!(
        lit.snapshot(),
        0,
        "the pedal released it, so the light goes"
    );
}

#[test]
fn a_device_that_goes_away_takes_its_lights_with_it() {
    // `release_all` is what a disconnect runs (TDD §14.2). A stuck light is
    // the visible half of a stuck note, and both come from the same set.
    let (mut r, lit, mut out) = rig();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_ON, 64, 100], &mut out);
    r.release_all(&mut out);
    assert_eq!(lit.snapshot(), 0);
}

#[test]
fn a_note_that_is_filtered_out_never_lights() {
    // Outside the device's velocity window is not this device's note: nothing
    // sounds, so nothing may light either.
    let settings = InputSettings {
        velocity_range: (64, 127),
        ..InputSettings::default()
    };
    let lit = Arc::new(LiveKeys::default());
    let mut r = MidiRouter::new(node(), 0, DeviceMapping::default())
        .watching_keys(Arc::clone(&lit))
        .following_input(Arc::new(LiveMapping::new(settings)));
    let mut out = Sink;

    r.handle(&[NOTE_ON, 60, 20], &mut out);
    assert_eq!(lit.snapshot(), 0);
}

#[test]
fn two_keyboards_share_one_set_of_lights() {
    // Every device the hub opens writes into the same cell — there is one
    // keyboard on screen, not one per controller. One device letting go of
    // its note must not put out the other's.
    let lit = Arc::new(LiveKeys::default());
    let mut one =
        MidiRouter::new(node(), 0, DeviceMapping::default()).watching_keys(Arc::clone(&lit));
    let mut two =
        MidiRouter::new(node(), 0, DeviceMapping::default()).watching_keys(Arc::clone(&lit));
    let mut out = Sink;

    one.handle(&[NOTE_ON, 60, 100], &mut out);
    two.handle(&[NOTE_ON, 72, 100], &mut out);
    assert_eq!(lit.snapshot(), (1u128 << 60) | (1u128 << 72));

    one.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(
        lit.snapshot(),
        1u128 << 72,
        "the other keyboard is still holding its note"
    );
}

#[test]
fn a_router_with_no_cell_to_write_into_still_plays() {
    // Every offline path builds a router without one — a headless render has
    // no keyboard to light — and none of them may pay for the window.
    let mut r = MidiRouter::new(node(), 0, DeviceMapping::default());
    let mut out = Sink;
    assert_eq!(r.handle(&[NOTE_ON, 60, 100], &mut out), 1);
    assert_eq!(r.handle(&[NOTE_OFF, 60, 0], &mut out), 1);
}
