//! The one part of live MIDI that no amount of unit testing reaches: `midir`
//! actually handing bytes over from a real port.
//!
//! Not run by `cargo test` — it opens real MIDI devices. Run it deliberately:
//! ```text
//! cargo test -p fontelle-midi --test hardware_loopback -- --ignored --nocapture
//! ```
//!
//! It needs a loopback: an ALSA "Midi Through" port (present by default on
//! Linux), or any device whose output is wired back to its input. Sending into
//! one and receiving it through `MidiHub` exercises exactly the path a
//! keyboard takes — enumeration, `connect`, the backend's callback thread, the
//! decode, the router, the queue — and nothing simulated.

use std::sync::{Arc, Mutex, MutexGuard};

use fontelle_midi::{MidiHub, RouteTo};
use fontelle_types::{EventPayload, EventSink, NodeId, TimedEvent};

/// Stands in for the engine's lock-free queue. A mutex is fine here and would
/// not be in the real thing: this is a test observer, not the audio thread.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<TimedEvent>>>);

impl EventSink for Shared {
    fn send(&mut self, event: TimedEvent) -> bool {
        self.0.lock().unwrap().push(event);
        true
    }
}

/// The loopback port is one piece of shared hardware, and `cargo test` runs
/// tests in parallel: without this, one test's note-on is received by the
/// other's hub and released by the other's shutdown. That showed up as a test
/// receiving three events for the two it sent.
static PORT: Mutex<()> = Mutex::new(());

fn exclusive_port() -> MutexGuard<'static, ()> {
    // A panic in one test must not make the other unrunnable.
    PORT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The ALSA loopback every Linux box has, or anything else that echoes.
fn loopback_output() -> Option<(midir::MidiOutput, midir::MidiOutputPort)> {
    let output = midir::MidiOutput::new("Fontelle loopback test").ok()?;
    let port = output.ports().into_iter().find(|p| {
        output
            .port_name(p)
            .map(|name| name.contains("Midi Through"))
            .unwrap_or(false)
    })?;
    Some((output, port))
}

#[test]
#[ignore = "opens real MIDI ports; needs a loopback. Run deliberately."]
fn a_note_played_on_a_real_port_arrives_as_a_timed_event() {
    let _exclusive = exclusive_port();
    let Some((output, out_port)) = loopback_output() else {
        eprintln!("no 'Midi Through' port on this machine — nothing to loop back through");
        return;
    };
    let port_name = output.port_name(&out_port).unwrap();
    eprintln!("sending into: {port_name}");

    let observed = Shared::default();
    let target = NodeId::from(slotmap::KeyData::from_ffi(3));
    let mut hub = MidiHub::new(RouteTo {
        node: target,
        voice_context: 7,
    });

    let report = hub
        .poll(|| Some(Box::new(observed.clone()) as Box<dyn EventSink>))
        .expect("enumerating MIDI inputs");
    eprintln!("opened: {:?}", report.connected);
    assert!(
        !report.connected.is_empty(),
        "no MIDI inputs were opened at all"
    );

    let mut connection = output.connect(&out_port, "fontelle-test-out").unwrap();
    connection.send(&[0x90, 60, 100]).unwrap();
    connection.send(&[0x80, 60, 0]).unwrap();

    // The backend delivers on its own thread; give it a moment rather than
    // racing it.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let events = observed.0.lock().unwrap().clone();
    eprintln!("received {} events", events.len());
    let notes: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::NoteOn { key, velocity, .. } => Some(("on", *key, *velocity)),
            EventPayload::NoteOff { key, .. } => Some(("off", *key, 0)),
            _ => None,
        })
        .collect();

    assert_eq!(
        notes,
        vec![("on", 60, 100), ("off", 60, 0)],
        "exactly what was sent, in order, and nothing else"
    );
    assert!(
        events.iter().all(|e| e.target == target),
        "every live event must carry the node its device is routed to"
    );

    hub.shutdown();
}

#[test]
#[ignore = "opens real MIDI ports; needs a loopback. Run deliberately."]
fn closing_a_device_that_is_holding_a_note_releases_it() {
    // TDD §14.2, against a real port: press a key, pull the cable. The
    // note-off can only come from the hub, because the device is gone.
    let _exclusive = exclusive_port();
    let Some((output, out_port)) = loopback_output() else {
        eprintln!("no 'Midi Through' port on this machine");
        return;
    };

    let observed = Shared::default();
    let mut hub = MidiHub::new(RouteTo {
        node: NodeId::from(slotmap::KeyData::from_ffi(3)),
        voice_context: 7,
    });
    hub.poll(|| Some(Box::new(observed.clone()) as Box<dyn EventSink>))
        .expect("enumerating MIDI inputs");

    let mut connection = output.connect(&out_port, "fontelle-test-out").unwrap();
    connection.send(&[0x90, 64, 100]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));

    observed.0.lock().unwrap().clear();
    hub.shutdown();

    let released: Vec<u8> = observed
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::NoteOff { key, .. } => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(
        released,
        vec![64],
        "the held note must be released when its device goes away"
    );
}
