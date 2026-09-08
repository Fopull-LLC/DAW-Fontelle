//! What a live device does to the event stream, without a live device.

use fontelle_midi::{DeviceMapping, MidiRouter, VelocityCurve};
use fontelle_types::{EventPayload, EventSink, NodeId, TimedEvent};

/// Collects instead of queueing, so a test can look at what a router produced.
#[derive(Default)]
struct Recorder {
    events: Vec<TimedEvent>,
    /// When set, every send is refused — a full queue.
    refuse: bool,
}

impl EventSink for Recorder {
    fn send(&mut self, event: TimedEvent) -> bool {
        if self.refuse {
            return false;
        }
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

fn target() -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(7))
}

fn router(mapping: DeviceMapping) -> MidiRouter {
    MidiRouter::new(target(), 42, mapping)
}

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;
const CC: u8 = 0xB0;

#[test]
fn a_key_press_and_release_become_a_note_on_and_a_note_off() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);

    assert_eq!(out.notes(), vec![("on", 60, 100), ("off", 60, 0)]);
    assert!(r.is_silent());
}

#[test]
fn live_events_carry_the_routers_target_and_voice_context() {
    // The target is what stops a keyboard playing every instrument at once;
    // the voice context is what stops the arrangement's note-offs cutting the
    // player's notes (TDD §11.4).
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 100], &mut out);

    assert_eq!(out.events[0].target, target());
    assert!(matches!(
        out.events[0].payload,
        EventPayload::NoteOn {
            voice_context: 42,
            ..
        }
    ));
}

#[test]
fn unplugging_a_device_releases_the_notes_it_was_holding() {
    // TDD §14.2: disconnecting "does not produce errors or stuck notes". A
    // held note whose device is gone can never receive its note-off, so
    // nothing else in the system is able to fix this.
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_ON, 64, 100], &mut out);
    r.handle(&[NOTE_ON | 3, 67, 100], &mut out);

    out.events.clear();
    r.release_all(&mut out);

    let mut released: Vec<u8> = out
        .notes()
        .iter()
        .map(|(kind, key, _)| {
            assert_eq!(*kind, "off");
            *key
        })
        .collect();
    released.sort_unstable();
    assert_eq!(released, vec![60, 64, 67], "every channel, every held key");
    assert!(r.is_silent());
}

#[test]
fn releasing_a_device_that_holds_nothing_sends_nothing() {
    // Otherwise every unplug sprays note-offs for all 128 keys on all 16
    // channels at whatever the device was routed to.
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    out.events.clear();

    assert_eq!(r.release_all(&mut out), 0);
    assert!(out.events.is_empty());
}

#[test]
fn all_notes_off_releases_only_the_channel_it_names() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_ON | 5, 72, 100], &mut out);
    out.events.clear();

    r.handle(&[CC, 123, 0], &mut out);
    assert_eq!(out.notes(), vec![("off", 60, 0)]);
    assert!(!r.is_silent(), "channel 5 is still holding its note");
}

#[test]
fn the_sustain_pedal_defers_note_offs_until_it_is_lifted() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();

    r.handle(&[CC, 64, 127], &mut out);
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(
        out.notes(),
        vec![("on", 60, 100)],
        "the key is up but the note is still sounding"
    );

    r.handle(&[CC, 64, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 60, 100), ("off", 60, 0)]);
    assert!(r.is_silent());
}

#[test]
fn a_pedal_below_the_halfway_point_is_up() {
    // The MIDI threshold is 64, not 1. A continuous pedal sweeping through 40
    // on its way down is not yet holding anything, and reading any non-zero
    // value as "down" makes such a pedal appear stuck.
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();

    r.handle(&[CC, 64, 63], &mut out);
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 60, 100), ("off", 60, 0)]);
}

#[test]
fn retriggering_a_pedalled_note_does_not_release_it_twice() {
    // Play a key, let it up with the pedal down, play it again, then lift the
    // pedal. The second note must survive: it is still held. A router that
    // left the first note's bit in the sustained set releases the note the
    // player is currently holding, the moment they lift the pedal.
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();

    r.handle(&[CC, 64, 127], &mut out);
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    r.handle(&[NOTE_ON, 60, 110], &mut out);
    out.events.clear();

    r.handle(&[CC, 64, 0], &mut out);
    assert!(
        out.notes().is_empty(),
        "lifting the pedal must not cut the key that is still down, got {:?}",
        out.notes()
    );
    assert!(!r.is_silent());

    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("off", 60, 0)]);
    assert!(r.is_silent());
}

#[test]
fn a_note_off_for_a_note_this_device_never_played_is_dropped() {
    // Live input and the timeline share an instrument. A stray note-off
    // forwarded blindly releases whichever voice happens to match — most
    // likely the arrangement's, not the player's.
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();

    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert!(out.events.is_empty());
}

#[test]
fn a_channel_filter_ignores_every_other_channel() {
    let mut r = router(DeviceMapping {
        channel_filter: Some(3),
        ..DeviceMapping::default()
    });
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON | 3, 60, 100], &mut out);
    r.handle(&[NOTE_ON | 4, 64, 100], &mut out);
    assert_eq!(out.notes(), vec![("on", 60, 100)]);
}

#[test]
fn transpose_moves_the_note_off_as_well_as_the_note_on() {
    // The half that gets forgotten, and the failure is a permanently stuck
    // note rather than a wrong pitch.
    let mut r = router(DeviceMapping {
        transpose_semitones: 12,
        ..DeviceMapping::default()
    });
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 72, 100), ("off", 72, 0)]);
    assert!(r.is_silent());
}

#[test]
fn a_note_transposed_off_the_keyboard_is_dropped_rather_than_wrapped() {
    let mut r = router(DeviceMapping {
        transpose_semitones: 24,
        ..DeviceMapping::default()
    });
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 120, 100], &mut out);
    assert!(
        out.events.is_empty(),
        "144 is not a key; wrapping to 16 would play a bass note"
    );
}

#[test]
fn a_remapped_pad_plays_the_key_it_was_pointed_at() {
    // TDD §14.3's drum-pad workflow: click the target key, hit the pad.
    let mut mapping = DeviceMapping::default();
    mapping.note_remap.insert(36, 60);
    let mut r = router(mapping);
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 36, 100], &mut out);
    r.handle(&[NOTE_OFF, 36, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 60, 100), ("off", 60, 0)]);
}

#[test]
fn a_velocity_range_excludes_the_notes_outside_it_and_their_note_offs() {
    let mut r = router(DeviceMapping {
        velocity_range: (64, 127),
        ..DeviceMapping::default()
    });
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 40], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert!(
        out.events.is_empty(),
        "a note the device filtered out must not produce a lone note-off"
    );
}

#[test]
fn the_velocity_curves_keep_full_scale_and_bend_the_middle() {
    for curve in [VelocityCurve::Soft, VelocityCurve::Hard] {
        assert_eq!(
            curve.apply(127),
            127,
            "{curve:?} must still reach full — a curve that cannot play loud is a volume control"
        );
    }
    assert_eq!(VelocityCurve::Linear.apply(64), 64);
    assert!(
        VelocityCurve::Soft.apply(64) > 64,
        "soft makes quiet playing louder"
    );
    assert!(
        VelocityCurve::Hard.apply(64) < 64,
        "hard makes it take more effort"
    );
    assert_eq!(VelocityCurve::Fixed(100).apply(1), 100);
}

#[test]
fn no_curve_can_turn_a_played_note_into_a_silent_one() {
    // `Hard` squares a normalised velocity, so the softest playable note
    // rounds to zero — and a velocity of zero read anywhere downstream is a
    // note-off, not a quiet note. The floor is what stops a curve making the
    // bottom of the keyboard dead.
    for curve in [
        VelocityCurve::Linear,
        VelocityCurve::Soft,
        VelocityCurve::Hard,
    ] {
        for velocity in 1..=127u8 {
            let out = curve.apply(velocity);
            assert!(
                (1..=127).contains(&out),
                "{curve:?} mapped velocity {velocity} to {out}"
            );
        }
    }
}

#[test]
fn the_velocity_curves_never_go_backwards() {
    // Playing harder must never produce a quieter note, whatever the shape.
    for curve in [VelocityCurve::Soft, VelocityCurve::Hard] {
        for velocity in 2..=127u8 {
            assert!(
                curve.apply(velocity) >= curve.apply(velocity - 1),
                "{curve:?} dips between {} and {velocity}",
                velocity - 1
            );
        }
    }
}

#[test]
fn a_full_queue_is_reported_rather_than_retried() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder {
        refuse: true,
        ..Recorder::default()
    };
    assert_eq!(
        r.handle(&[NOTE_ON, 60, 100], &mut out),
        0,
        "nothing was queued"
    );
}

#[test]
fn active_sensing_and_clock_produce_nothing() {
    // A keyboard sends these continuously for as long as it is plugged in.
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    for _ in 0..100 {
        r.handle(&[0xFE], &mut out);
        r.handle(&[0xF8], &mut out);
    }
    assert!(out.events.is_empty());
}

// ------------------------------- the wheels reach the instrument (2026-09-05)

/// A mod wheel is forwarded as a **controller** event, on the router's
/// current target.
///
/// Until now every controller but the sustain pedal was dropped at the
/// router — *"a keyboard's wheels reach nothing"* — because the nodes they
/// would address exposed no parameters. Hosted plugins do, and so do the
/// built-in instruments soon; the router now forwards what it decodes, as
/// performance events rather than as automation (`EventPayload::Controller`
/// is not a `ParamValue`: it has no §8.2 address, it is what a hand did).
#[test]
fn a_mod_wheel_is_forwarded_as_a_controller() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    assert_eq!(r.handle(&[CC, 1, 100], &mut out), 1);
    assert_eq!(out.events.len(), 1);
    assert!(
        matches!(
            out.events[0].payload,
            EventPayload::Controller {
                controller: 1,
                value: 100
            }
        ),
        "{:?}",
        out.events[0].payload
    );
    assert_eq!(out.events[0].target, target());
}

/// Pitch bend arrives centred at zero, the way the decoder reads it.
#[test]
fn a_pitch_bend_is_forwarded_centred_at_zero() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[0xE0, 0x00, 0x40], &mut out);
    r.handle(&[0xE0, 0x7F, 0x7F], &mut out);
    r.handle(&[0xE0, 0x00, 0x00], &mut out);
    let bends: Vec<i16> = out
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::PitchBend { value } => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(bends, vec![0, 8191, -8192]);
}

/// Channel pressure — aftertouch — is forwarded too.
#[test]
fn channel_pressure_is_forwarded() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[0xD0, 64], &mut out);
    assert!(matches!(
        out.events[0].payload,
        EventPayload::ChannelPressure { value: 64 }
    ));
}

/// The sustain pedal is **still the router's own**: it defers note-offs
/// rather than being forwarded, exactly as before, so an instrument never
/// sees CC 64 and the router's bookkeeping stays the one that holds notes.
#[test]
fn the_sustain_pedal_is_not_forwarded_as_a_controller() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    assert_eq!(r.handle(&[CC, 64, 127], &mut out), 0);
    assert_eq!(r.handle(&[CC, 64, 0], &mut out), 0);
    assert!(out.events.is_empty(), "{:?}", out.events);
}

/// A controller on a channel the device filter excludes goes nowhere, like
/// a note on that channel.
#[test]
fn a_controller_on_a_filtered_channel_is_dropped() {
    let mut r = router(DeviceMapping {
        channel_filter: Some(2),
        ..DeviceMapping::default()
    });
    let mut out = Recorder::default();
    assert_eq!(r.handle(&[CC | 5, 1, 100], &mut out), 0);
    assert_eq!(r.handle(&[CC | 2, 1, 100], &mut out), 1);
}

/// And a program change still goes nowhere: nothing here takes one, and an
/// event nothing reads would look like a working feature.
#[test]
fn a_program_change_still_goes_nowhere() {
    let mut r = router(DeviceMapping::default());
    let mut out = Recorder::default();
    assert_eq!(r.handle(&[0xC0, 5], &mut out), 0);
}
