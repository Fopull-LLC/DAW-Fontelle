//! The live-input boundary: where events that did not come from the timeline —
//! a MIDI keyboard, a controller, a UI keyboard — reach the audio thread
//! (TDD §14.1).
//!
//! §14.1 is unusually specific about this, and it is worth repeating why: the
//! scaffold must **already** be the RT-safe, device-agnostic pipeline, feeding
//! the same sample-timestamped event stream as notes and automation. The
//! tempting shortcut — poll MIDI on the UI thread and call `note_on` directly —
//! works immediately and then has to be torn out, because everything built on
//! top of it assumes events can arrive from anywhere at any time.
//!
//! **Why a fixed array of single-producer queues rather than one shared one.**
//! `rtrb` is SPSC, and "all input devices are merged into one logical stream"
//! (§14.2) means several device threads producing at once — which a single
//! SPSC queue cannot take, and which a mutex could take only by letting a
//! device thread block the audio thread. So each port gets a queue of its own
//! and the merge happens at the drain. The array is allocated up front at a
//! fixed size because the alternative is the audio thread walking a collection
//! that hot-plug is mutating underneath it: claiming a port for a newly
//! connected device is then a hand-off of an already-existing queue, and the
//! consumer's structure never changes.

use fontelle_types::{EventSink, Sample, TimedEvent};

/// How many devices can be connected at once. Sixteen is far past what anyone
/// plugs in and small enough that draining every slot per block is a handful
/// of atomic loads.
pub const LIVE_PORT_COUNT: usize = 16;

/// Events a single port can hold between two audio blocks. At 128 frames /
/// 48 kHz that is 2.7 ms; 256 events in 2.7 ms is ~95 000 messages a second,
/// which no controller and no fast glissando comes close to.
pub const LIVE_PORT_CAPACITY: usize = 256;

/// Builds the live-input channel: the consumer half for the audio thread and
/// the producer halves for whatever is generating events.
pub fn live_event_channel(ports: usize, capacity: usize) -> (LiveEventSource, LiveEventPorts) {
    let mut consumers = Vec::with_capacity(ports);
    let mut producers = Vec::with_capacity(ports);
    for _ in 0..ports {
        let (producer, consumer) = rtrb::RingBuffer::new(capacity);
        consumers.push(consumer);
        producers.push(Some(producer));
    }
    (
        LiveEventSource {
            consumers,
            scratch: Vec::with_capacity(ports * capacity),
            capture: None,
        },
        LiveEventPorts { producers },
    )
}

/// Events a recording can hold before the model thread empties it. At 128
/// frames / 48 kHz the model side gets a chance every 2.7 ms, so this is
/// roughly a hundred blocks of the busiest playing anyone does — generous
/// enough that a UI thread stalling on a file dialog does not cost a take.
pub const CAPTURE_CAPACITY: usize = 8_192;

/// Builds the recording channel: the half the audio thread writes into, and
/// the half the model thread empties.
///
/// The other direction from [`live_event_channel`], and for the same reason —
/// the audio thread must never block or allocate, so the ring is preallocated
/// and a full one drops rather than waits.
pub fn live_capture_channel(capacity: usize) -> (CaptureWriter, CaptureReader) {
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        CaptureWriter {
            producer,
            dropped: dropped.clone(),
        },
        CaptureReader { consumer, dropped },
    )
}

/// The audio thread's half of a recording.
pub struct CaptureWriter {
    producer: rtrb::Producer<TimedEvent>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

/// The model thread's half of a recording.
pub struct CaptureReader {
    consumer: rtrb::Consumer<TimedEvent>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl CaptureReader {
    /// Moves everything captured so far into `out`, keeping what is already
    /// there. Off the RT thread, so growing `out` is fine.
    pub fn drain_into(&mut self, out: &mut Vec<TimedEvent>) {
        while let Ok(event) = self.consumer.pop() {
            out.push(event);
        }
    }

    /// How many events the ring had to drop because nobody emptied it.
    ///
    /// Not zero is a take with holes in it, which the user has to be told
    /// about — a recording that quietly lost notes is worse than one that
    /// failed.
    pub fn dropped(&self) -> usize {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The audio thread's half. Owned by the callback, drained once per block.
pub struct LiveEventSource {
    consumers: Vec<rtrb::Consumer<TimedEvent>>,
    /// Preallocated to hold every port's full capacity, so a drain can never
    /// need to grow it (INVARIANT 1).
    scratch: Vec<TimedEvent>,
    /// Armed while a recording is in progress. Every live event that reaches
    /// the graph is mirrored into it, which is the whole of "record what I
    /// play": the take is a copy of the stream that made the sound, not a
    /// second reading of the device.
    capture: Option<CaptureWriter>,
}

impl LiveEventSource {
    /// RT: collects everything every port has queued, stamped at `sample`.
    ///
    /// **All at the block's start, not at their true arrival time.** A live
    /// event's real timestamp would have to be comparable with the audio
    /// clock, and what a MIDI backend hands over is its own monotonic clock
    /// with no published relationship to the one driving the callback.
    /// Guessing at the conversion buys sample accuracy that is wrong by an
    /// unknown offset; stamping at the block start is wrong by at most one
    /// block — 2.7 ms at 128 frames — in a known direction, always late,
    /// never early. Sample-accurate live input is a real feature and it needs
    /// the device timestamps first.
    /// `recording` mirrors every event into the armed capture on the way
    /// past. Deliberately the same events, already stamped: a take is a copy
    /// of the stream that made the sound, so what is written down and what was
    /// heard cannot drift apart.
    pub fn drain(&mut self, sample: Sample, recording: bool) -> &[TimedEvent] {
        self.scratch.clear();
        for consumer in self.consumers.iter_mut() {
            while self.scratch.len() < self.scratch.capacity() {
                match consumer.pop() {
                    Ok(mut event) => {
                        event.sample = sample;
                        self.scratch.push(event);
                    }
                    Err(_) => break,
                }
            }
        }
        if recording && let Some(capture) = self.capture.as_mut() {
            for event in &self.scratch {
                let Some(copy) = rt_safe_copy(event) else {
                    continue;
                };
                if capture.producer.push(copy).is_err() {
                    // Nobody is emptying it. Dropping is the only option that
                    // does not block the audio thread; counting is what lets
                    // the user be told the take has holes in it.
                    capture
                        .dropped
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        &self.scratch
    }

    /// Starts mirroring live events into `writer`.
    pub fn arm_capture(&mut self, writer: CaptureWriter) {
        self.capture = Some(writer);
    }

    /// Stops mirroring, handing the writer back.
    pub fn disarm_capture(&mut self) -> Option<CaptureWriter> {
        self.capture.take()
    }
}

/// Duplicates an event without allocating, or gives up.
///
/// `EventPayload::ParamValue` carries an owned `ParamAddress`, and cloning a
/// `String` on the audio thread is an allocation — INVARIANT 1, and the guard
/// would say so. Nothing live produces one today: CC routing beyond sustain is
/// not built, by design, because the nodes it would address expose no
/// parameters yet. When it is, the address wants to be a `Copy` handle rather
/// than a string, which §8.2's stable-id table gives it anyway.
fn rt_safe_copy(event: &TimedEvent) -> Option<TimedEvent> {
    let payload = match &event.payload {
        fontelle_types::EventPayload::NoteOn {
            key,
            velocity,
            pan,
            fine_pitch,
            release,
            mod_x,
            mod_y,
            voice_context,
        } => fontelle_types::EventPayload::NoteOn {
            key: *key,
            velocity: *velocity,
            pan: *pan,
            fine_pitch: *fine_pitch,
            release: *release,
            mod_x: *mod_x,
            mod_y: *mod_y,
            voice_context: *voice_context,
        },
        fontelle_types::EventPayload::NoteOff { key, voice_context } => {
            fontelle_types::EventPayload::NoteOff {
                key: *key,
                voice_context: *voice_context,
            }
        }
        fontelle_types::EventPayload::NoteSlide {
            key,
            glide_samples,
            voice_context,
        } => fontelle_types::EventPayload::NoteSlide {
            key: *key,
            glide_samples: *glide_samples,
            voice_context: *voice_context,
        },
        fontelle_types::EventPayload::ClipStart => fontelle_types::EventPayload::ClipStart,
        fontelle_types::EventPayload::ClipStop => fontelle_types::EventPayload::ClipStop,
        // Not captured: a recording is notes (`fontelle_model::recording`),
        // and a wheel move written into a take would be a controller lane
        // this program cannot draw yet. The instrument still hears it live.
        fontelle_types::EventPayload::ParamValue { .. }
        | fontelle_types::EventPayload::Controller { .. }
        | fontelle_types::EventPayload::PitchBend { .. }
        | fontelle_types::EventPayload::ChannelPressure { .. }
        | fontelle_types::EventPayload::NoteMod { .. } => return None,
    };
    Some(TimedEvent {
        sample: event.sample,
        target: event.target,
        payload,
    })
}

/// The non-RT half: a pool of unclaimed producers, one per port.
pub struct LiveEventPorts {
    producers: Vec<Option<rtrb::Producer<TimedEvent>>>,
}

impl LiveEventPorts {
    /// Hands out one port's producer, or `None` when every port is in use.
    pub fn claim(&mut self) -> Option<LivePort> {
        self.producers
            .iter_mut()
            .find_map(|slot| slot.take())
            .map(|producer| LivePort { producer })
    }

    /// How many ports are still free.
    pub fn available(&self) -> usize {
        self.producers.iter().filter(|p| p.is_some()).count()
    }
}

/// One device's (or one source's) way in. `Send`, so it goes to whatever
/// thread the device's callback runs on.
pub struct LivePort {
    producer: rtrb::Producer<TimedEvent>,
}

impl EventSink for LivePort {
    /// Returns `false` when the port is full, which means the audio thread has
    /// stopped draining — the device stream died, or the process is being torn
    /// down. Dropping the event is the only option that does not block a
    /// device callback, and a caller that cares can count the refusals.
    fn send(&mut self, event: TimedEvent) -> bool {
        self.producer.push(event).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontelle_types::{EventPayload, NodeId};

    fn note_on(key: u8) -> TimedEvent {
        TimedEvent {
            // Deliberately wrong: the drain is what assigns a live event its
            // place in the song, and a test that pre-stamped them correctly
            // would not notice if it stopped.
            sample: -1,
            target: NodeId::default(),
            payload: EventPayload::NoteOn {
                key,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            },
        }
    }

    #[test]
    fn an_event_sent_between_blocks_arrives_stamped_at_the_next_blocks_start() {
        let (mut source, mut ports) = live_event_channel(4, 8);
        let mut port = ports.claim().expect("a free port");

        assert!(port.send(note_on(60)));
        let events = source.drain(4_096, false);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sample, 4_096);
    }

    #[test]
    fn every_port_is_merged_into_one_stream() {
        // TDD §14.2: all input devices merged automatically, no selection
        // step. The merge happens here, at the drain.
        let (mut source, mut ports) = live_event_channel(4, 8);
        let mut first = ports.claim().unwrap();
        let mut second = ports.claim().unwrap();

        first.send(note_on(60));
        second.send(note_on(64));
        first.send(note_on(67));

        let events = source.drain(0, false);
        assert_eq!(events.len(), 3, "three keys down across two keyboards");
    }

    #[test]
    fn a_drained_event_is_not_delivered_twice() {
        let (mut source, mut ports) = live_event_channel(2, 8);
        let mut port = ports.claim().unwrap();
        port.send(note_on(60));

        assert_eq!(source.drain(0, false).len(), 1);
        assert_eq!(
            source.drain(128, false).len(),
            0,
            "a note held down is one note-on, not one per block"
        );
    }

    // --- Recording (TDD §14.7) --------------------------------------------

    #[test]
    fn a_live_event_is_mirrored_into_the_capture_while_recording() {
        // A take is a copy of the stream that made the sound, not a second
        // reading of the device — so what was written down and what was heard
        // cannot drift apart.
        let (mut source, mut ports) = live_event_channel(2, 8);
        let (writer, mut reader) = live_capture_channel(16);
        source.arm_capture(writer);
        let mut port = ports.claim().unwrap();

        port.send(note_on(60));
        let heard = source.drain(24_000, true);
        assert_eq!(heard.len(), 1);

        let mut captured = Vec::new();
        reader.drain_into(&mut captured);
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].sample, 24_000,
            "the take has to know where in the song the note was played"
        );
        assert!(matches!(
            captured[0].payload,
            EventPayload::NoteOn { key: 60, .. }
        ));
    }

    #[test]
    fn nothing_is_captured_when_the_transport_is_not_recording() {
        // Playing along without the record button down must leave no take
        // behind, or the next one starts with somebody else's warm-up in it.
        let (mut source, mut ports) = live_event_channel(2, 8);
        let (writer, mut reader) = live_capture_channel(16);
        source.arm_capture(writer);
        let mut port = ports.claim().unwrap();

        port.send(note_on(60));
        assert_eq!(source.drain(0, false).len(), 1, "it still sounds");

        let mut captured = Vec::new();
        reader.drain_into(&mut captured);
        assert!(captured.is_empty());
    }

    #[test]
    fn disarming_stops_the_capture_and_hands_the_writer_back() {
        let (mut source, mut ports) = live_event_channel(2, 8);
        let (writer, mut reader) = live_capture_channel(16);
        source.arm_capture(writer);
        let mut port = ports.claim().unwrap();

        port.send(note_on(60));
        source.drain(0, true);
        assert!(source.disarm_capture().is_some());
        port.send(note_on(64));
        source.drain(128, true);

        let mut captured = Vec::new();
        reader.drain_into(&mut captured);
        assert_eq!(captured.len(), 1, "only the note played while armed");
    }

    #[test]
    fn a_full_capture_drops_and_counts_rather_than_blocking_the_audio_thread() {
        // The same rule as the input side: an audio thread must never wait for
        // a model thread. A take with holes in it is something the user has to
        // be told about, though — a recording that quietly lost notes is worse
        // than one that failed.
        let (mut source, mut ports) = live_event_channel(1, 8);
        let (writer, mut reader) = live_capture_channel(2);
        source.arm_capture(writer);
        let mut port = ports.claim().unwrap();

        for key in 60..65 {
            port.send(note_on(key));
        }
        source.drain(0, true);

        let mut captured = Vec::new();
        reader.drain_into(&mut captured);
        assert_eq!(captured.len(), 2, "the ring held two");
        assert_eq!(reader.dropped(), 3, "and said so about the other three");
    }

    #[test]
    fn recording_does_not_change_what_the_audio_thread_is_handed() {
        let (mut source, mut ports) = live_event_channel(2, 8);
        let mut port = ports.claim().unwrap();
        port.send(note_on(60));
        let without: Vec<i64> = source.drain(0, true).iter().map(|e| e.sample).collect();

        let (mut armed, mut ports) = live_event_channel(2, 8);
        let (writer, _reader) = live_capture_channel(16);
        armed.arm_capture(writer);
        let mut port = ports.claim().unwrap();
        port.send(note_on(60));
        let with: Vec<i64> = armed.drain(0, true).iter().map(|e| e.sample).collect();

        assert_eq!(without, with);
    }

    #[test]
    fn a_parameter_event_is_left_out_rather_than_allocating_on_the_audio_thread() {
        // `ParamValue` carries an owned address, and cloning a `String` on the
        // audio thread is an allocation the guard would reject. Nothing live
        // produces one today; this pins the reason so the next person to route
        // a CC finds it.
        let (mut source, mut ports) = live_event_channel(1, 8);
        let (writer, mut reader) = live_capture_channel(16);
        source.arm_capture(writer);
        let mut port = ports.claim().unwrap();

        port.send(TimedEvent {
            sample: 0,
            target: NodeId::default(),
            payload: EventPayload::ParamValue {
                target: fontelle_types::ParamAddress::new("transport/tempo"),
                value: 128.0,
            },
        });
        port.send(note_on(60));
        source.drain(0, true);

        let mut captured = Vec::new();
        reader.drain_into(&mut captured);
        assert_eq!(captured.len(), 1);
        assert!(matches!(captured[0].payload, EventPayload::NoteOn { .. }));
    }

    #[test]
    fn a_full_port_refuses_rather_than_blocking_the_device_thread() {
        let (_source, mut ports) = live_event_channel(1, 2);
        let mut port = ports.claim().unwrap();

        assert!(port.send(note_on(60)));
        assert!(port.send(note_on(61)));
        assert!(
            !port.send(note_on(62)),
            "the audio thread has stopped draining; a device callback must not wait for it"
        );
    }

    #[test]
    fn ports_run_out_rather_than_growing() {
        let (_source, mut ports) = live_event_channel(2, 4);
        assert_eq!(ports.available(), 2);
        assert!(ports.claim().is_some());
        assert!(ports.claim().is_some());
        assert!(
            ports.claim().is_none(),
            "growing the pool would mean reallocating a structure the audio thread walks"
        );
        assert_eq!(ports.available(), 0);
    }

    #[test]
    fn a_drain_never_returns_more_than_the_scratch_can_hold() {
        // The scratch is sized for every port's full capacity, so this is the
        // ceiling rather than a truncation anyone should hit — but the bound
        // is what makes the drain allocation-free, so it is worth pinning.
        let (mut source, mut ports) = live_event_channel(2, 4);
        let mut a = ports.claim().unwrap();
        let mut b = ports.claim().unwrap();
        for key in 0..4 {
            a.send(note_on(key));
            b.send(note_on(key));
        }
        assert_eq!(source.drain(0, false).len(), 8);
    }
}

/// Decides whether the graph has to run while the transport is stopped
/// (TDD §6.3 against §14).
///
/// §6.3 says a stopped transport does not process the graph, and that is what
/// delivers the near-zero idle CPU target. Taken literally it also means a
/// keyboard makes no sound unless the song is rolling, which is not a DAW —
/// auditioning an instrument with the transport stopped is most of what a
/// sampler is *for*.
///
/// The reconciliation is that "stopped" should mean *idle*, and idle means
/// nothing is making sound. A live event wakes the graph; after that it stays
/// awake for exactly as long as its output is non-silent, which covers a note
/// held indefinitely and a release tail alike, and returns to true idle on its
/// own the moment the sound stops. The alternative — a fixed timeout after the
/// last event — cuts a held pad off mid-note, and asking the nodes whether
/// they are silent means every node has to answer honestly for this to work at
/// all.
pub struct IdleGate {
    ringing: bool,
    /// How many notes live input is holding down right now.
    ///
    /// **The measurement alone is not enough, and this is the field that
    /// says why.** Every enveloped instrument starts from zero, so the block
    /// a note-on arrives in is regularly quieter than the floor below —
    /// captured on real hardware, exactly `0.0`. A gate that asked only what
    /// it had just heard therefore put the graph to sleep underneath the note
    /// it had that instant started, and the note became audible for two
    /// blocks only when the *note-off* woke the graph again. Held down for
    /// half a second, played back as a click: *"it just does a flicker"*.
    ///
    /// A key that is down is a fact rather than a measurement, so it is
    /// counted rather than inferred. It also covers the case the measurement
    /// can never cover: an instrument with a two-second attack, which is
    /// silent for hundreds of blocks and going to sound for all of them.
    ///
    /// A dropped note-off cannot strand this above zero without also
    /// stranding the voice it belongs to — and a voice nothing releases is
    /// audible, so `ringing` holds the graph awake anyway. There is no state
    /// here that outlives the sound it is standing in for.
    held: u32,
    /// Whether an input stream is open and being monitored.
    ///
    /// A fourth reason to be awake, and the only one that is neither an event
    /// nor a measurement: *"i should be able to hear routed input playing even
    /// when song isnt playing or im not recording."* It cannot be inferred
    /// from the output, because the honest state of a microphone in a quiet
    /// room is silence — a gate that measured its way to sleep would swallow
    /// the first word spoken into it.
    monitoring: bool,
    /// Whether a plugin's own editor is open.
    ///
    /// A fifth reason, and like `monitoring` one that cannot be measured:
    /// an LV2 editor talks to its plugin only through `run` — the file it
    /// was handed rides an atom the plugin reads at the top of a block, and
    /// the *"I loaded it"* answer comes out of one. A gate that slept
    /// underneath an open editor was a sampler that could not be given a
    /// sample while the song was stopped, which is exactly when one is. Read
    /// off the transport each callback (`Transport::is_attended`), written by
    /// the window that owns the editors.
    attended: bool,
    /// Blocks still owed to live input before the measurement is believed.
    ///
    /// For the note whose note-on and note-off land in the *same* drain — a
    /// fast passage, or a file played into the port. `held` is back to zero
    /// before the graph has rendered a single sample of it, so the count
    /// alone would let a note be silenced by its own release.
    settling: u32,
}

/// How long that grace is. Thirty-two blocks is 85 ms at 128 frames / 48 kHz:
/// past the silent start of any attack worth the name, and short enough that
/// an idle window is back to costing nothing within a tenth of a second.
const SETTLING_BLOCKS: u32 = 32;

impl IdleGate {
    pub fn new() -> Self {
        Self {
            ringing: false,
            held: 0,
            monitoring: false,
            attended: false,
            settling: 0,
        }
    }

    /// Says whether a live input is open. See the field.
    pub fn set_monitoring(&mut self, on: bool) {
        self.monitoring = on;
    }

    /// Says whether a plugin's own editor is open. See the field.
    pub fn set_attended(&mut self, on: bool) {
        self.attended = on;
    }

    /// Takes account of this block's live input. Call once per callback with
    /// everything the drain produced, **before** [`is_awake`](Self::is_awake).
    ///
    /// It counts notes and nothing else: a controller message changes no
    /// note's fate, and the `live_events > 0` term of `is_awake` already
    /// wakes the graph for the block one arrives in.
    pub fn take_live(&mut self, events: &[TimedEvent]) {
        for event in events {
            match event.payload {
                fontelle_types::EventPayload::NoteOn { .. } => {
                    self.held += 1;
                    self.settling = SETTLING_BLOCKS;
                }
                fontelle_types::EventPayload::NoteOff { .. } => {
                    // Saturating, not signed: a note-off for something this
                    // gate never saw — a device opened mid-chord, a
                    // `release_all` after a drop — would otherwise leave the
                    // count negative, and the next real note-on would not
                    // lift it back above zero.
                    self.held = self.held.saturating_sub(1);
                    self.settling = SETTLING_BLOCKS;
                }
                _ => {}
            }
        }
    }

    /// Everything let go of at once — a full graph reset, a device teardown.
    pub fn release_all(&mut self) {
        self.held = 0;
        self.settling = 0;
    }

    /// Whether to run the graph despite a stopped transport.
    ///
    /// Five independent reasons, and they answer different questions:
    /// something arrived this block, something is being *played* and has not
    /// been let go, something is still *making sound*, a microphone is open
    /// and the graph is what carries it to the speakers, or somebody is at a
    /// plugin's own controls and the graph is what carries their words to it.
    pub fn is_awake(&self, live_events: usize) -> bool {
        live_events > 0
            || self.held > 0
            || self.settling > 0
            || self.ringing
            || self.monitoring
            || self.attended
    }

    /// Records what the block just rendered actually produced. `peak` is the
    /// largest absolute sample across the output buses.
    ///
    /// Measuring the output is the *only* input to this decision, deliberately.
    /// A "the graph was just reset, so nothing can be ringing" shortcut is
    /// wrong now that a reset is scoped: a stop cuts the sequenced voices and
    /// leaves a held key sounding, and a gate cleared on that reset would put
    /// the graph to sleep underneath the note it just spared.
    ///
    /// The threshold is -100 dBFS, below the noise floor of 16- and 24-bit
    /// audio alike, so nothing audible is ever cut short — but a decaying
    /// envelope that approaches zero asymptotically still crosses it, which a
    /// test for exact zero would not.
    pub fn observe(&mut self, peak: f32) {
        const SILENCE: f32 = 1e-5;
        self.ringing = peak > SILENCE;
        // Counted per *rendered* block rather than per callback: it is a
        // grace measured in chances to make a sound, and a callback that ran
        // nothing gave the graph no chance at all.
        self.settling = self.settling.saturating_sub(1);
    }
}

impl Default for IdleGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod idle_gate_tests {
    use super::*;
    use fontelle_types::{EventPayload, NodeId};

    fn key_event(payload: EventPayload) -> TimedEvent {
        TimedEvent {
            sample: 0,
            target: NodeId::default(),
            payload,
        }
    }

    fn on(key: u8) -> TimedEvent {
        key_event(EventPayload::NoteOn {
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        })
    }

    fn off(key: u8) -> TimedEvent {
        key_event(EventPayload::NoteOff {
            key,
            voice_context: 0,
        })
    }

    // --- somebody at a plugin's controls (2026-09-05) ----------------------

    /// An open plugin editor keeps the graph awake with nothing sounding.
    ///
    /// An LV2 editor talks to its plugin only through `run`: the file it was
    /// handed rides an atom the plugin reads at the top of a block, and the
    /// "I loaded it" answer comes out of one. A gate that slept underneath
    /// an open editor was a sampler that could never be given a sample
    /// while the song was stopped — which is exactly when one is.
    #[test]
    fn an_attended_plugin_keeps_the_graph_awake() {
        let mut gate = IdleGate::new();
        assert!(!gate.is_awake(0));
        gate.set_attended(true);
        assert!(gate.is_awake(0), "somebody is at the plugin's controls");
        gate.observe(0.0);
        assert!(gate.is_awake(0), "and silence does not put it to sleep");
        gate.set_attended(false);
        assert!(!gate.is_awake(0), "the editor closed: idle again");
    }

    // --- what a player is holding (reported from a real keyboard) ---------

    #[test]
    fn a_key_held_down_keeps_the_graph_awake_while_its_attack_is_still_silent() {
        // Reported from playing an Akai MPK mini: *"when I try playing a
        // single note it often doesn't play, it just does a flicker."*
        //
        // The measurement alone cannot answer this. Every enveloped
        // instrument starts from zero, so the block a note-on arrives in is
        // regularly quieter than the -100 dBFS floor — and the gate, asked
        // only what it had just heard, put the graph to sleep underneath the
        // note it had that instant started. Captured on hardware: the note-on
        // block measured exactly 0.0, the graph then idled for the whole 500 ms
        // the key was down, and the note became audible for two blocks only
        // when the *note-off* woke the graph again. That is the flicker.
        //
        // A key that is down is a fact, not a measurement, so it is counted.
        let mut gate = IdleGate::new();
        gate.take_live(&[on(60)]);
        assert!(gate.is_awake(1), "the block the note-on arrived in");
        gate.observe(0.0);
        assert!(
            gate.is_awake(0),
            "a key is still down: silence this block says nothing about the next"
        );
        for _ in 0..1_000 {
            gate.observe(0.0);
            assert!(gate.is_awake(0), "a long attack must not be slept through");
        }
    }

    #[test]
    fn letting_the_key_go_hands_the_decision_back_to_the_measurement() {
        // The release tail is exactly what the measurement is good at, and
        // the count must not keep the graph awake for ever after it.
        let mut gate = IdleGate::new();
        gate.take_live(&[on(60)]);
        gate.observe(0.0);
        gate.take_live(&[off(60)]);
        gate.observe(0.4);
        assert!(gate.is_awake(0), "the tail is still ringing");
        for _ in 0..64 {
            gate.observe(0.0);
        }
        assert!(!gate.is_awake(0), "and idle CPU has to come back down");
    }

    #[test]
    fn a_chord_is_awake_until_the_last_of_it_is_let_go() {
        let mut gate = IdleGate::new();
        gate.take_live(&[on(60), on(64), on(67)]);
        gate.take_live(&[off(60), off(64)]);
        gate.observe(0.0);
        assert!(gate.is_awake(0), "one key is still down");
        gate.take_live(&[off(67)]);
        for _ in 0..64 {
            gate.observe(0.0);
        }
        assert!(!gate.is_awake(0));
    }

    #[test]
    fn a_note_off_with_nothing_down_cannot_drive_the_count_below_nothing() {
        // A note-off for something this gate never saw — a device opened
        // mid-chord, a `release_all` after a drop. Left signed, it would make
        // the count negative and the *next* real note-on would not lift it
        // back above zero, which is the same bug again with an extra step.
        let mut gate = IdleGate::new();
        gate.take_live(&[off(60), off(64)]);
        for _ in 0..64 {
            gate.observe(0.0);
        }
        assert!(!gate.is_awake(0));
        gate.take_live(&[on(60)]);
        gate.observe(0.0);
        assert!(gate.is_awake(0), "the next note still has to wake it");
    }

    #[test]
    fn a_note_shorter_than_one_block_still_gets_a_chance_to_sound() {
        // Both halves of it can land in the same drain — a fast passage, or a
        // file replayed into the port. The count is back to zero before the
        // graph has rendered anything at all, so the count alone would let
        // this note be silenced by its own release.
        let mut gate = IdleGate::new();
        gate.take_live(&[on(60), off(60)]);
        gate.observe(0.0);
        assert!(
            gate.is_awake(0),
            "it has not been given a block to sound in"
        );
    }

    #[test]
    fn nothing_played_at_all_is_still_asleep() {
        // The whole point of the gate: a window sitting open costs nothing.
        let mut gate = IdleGate::new();
        gate.take_live(&[]);
        assert!(!gate.is_awake(0));
    }

    #[test]
    fn nothing_playing_leaves_the_graph_asleep() {
        let gate = IdleGate::new();
        assert!(!gate.is_awake(0), "an idle stopped transport runs nothing");
    }

    #[test]
    fn a_live_event_wakes_the_graph() {
        let gate = IdleGate::new();
        assert!(gate.is_awake(1));
    }

    #[test]
    fn a_note_keeps_the_graph_awake_after_the_event_that_started_it() {
        // The event arrives in one block and the note sounds for thousands.
        let mut gate = IdleGate::new();
        assert!(gate.is_awake(1));
        gate.observe(0.4);
        assert!(gate.is_awake(0), "the note is still sounding");
    }

    #[test]
    fn a_note_that_has_died_away_lets_the_graph_sleep_again() {
        let mut gate = IdleGate::new();
        gate.observe(0.4);
        assert!(gate.is_awake(0));
        gate.observe(0.0);
        assert!(!gate.is_awake(0), "idle CPU has to come back down");
    }

    #[test]
    fn a_tail_below_the_noise_floor_counts_as_silence() {
        // An exponential release approaches zero without reaching it. Waiting
        // for exact zero would keep the graph awake forever after one note.
        let mut gate = IdleGate::new();
        gate.observe(1e-9);
        assert!(!gate.is_awake(0));
    }
}
