//! MIDI into an insert (`docs/tune-plan.md` §5.2, §9.5).
//!
//! The one piece of plumbing the corrector adds to the DAW: an insert that
//! reads **another node's** note events, because it is configured to listen to
//! one channel's part. `ProcessContext::all_events` warns that a node reading
//! it will play other instruments' parts — and that is exactly the intent
//! here, which is why this file names what it must and must not hear.

use fontelle_engine::{
    AudioNode, EffectNode, PrepareContext, ProcessContext, TransportSnapshot, TransportState,
};
use fontelle_types::{EffectConfig, EventPayload, NodeId, TimedEvent, TuneConfig, TuneControl};

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

/// Three node ids that are not each other.
///
/// A `NodeId` is a slotmap key and the graph mints them; here they are built
/// through the round trip `fontelle_midi::LiveTarget` already relies on, which
/// is the only way to name one without a graph to take it from.
fn ids() -> (NodeId, NodeId, NodeId) {
    (
        NodeId::from_bits(1 << 32 | 1),
        NodeId::from_bits(1 << 32 | 2),
        NodeId::from_bits(1 << 32 | 3),
    )
}

fn note_on(target: NodeId, key: u8) -> TimedEvent {
    TimedEvent {
        sample: 0,
        target,
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

fn note_off(target: NodeId, key: u8) -> TimedEvent {
    TimedEvent {
        sample: 0,
        target,
        payload: EventPayload::NoteOff {
            key,
            voice_context: 0,
        },
    }
}

/// A tune insert listening to `source`, prepared and ready.
fn insert(source: NodeId) -> EffectNode {
    let config = EffectConfig::Tune(TuneConfig {
        control: TuneControl::MidiMelody,
        ..TuneConfig::new()
    });
    let mut node = EffectNode::new(config).with_notes_from(source);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    node
}

/// One block through the node, with the timeline's and the live events given
/// separately — which is how the graph hands them over.
fn run(node: &mut EffectNode, timeline: &[TimedEvent], live: &[TimedEvent], me: NodeId) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: &mut channels,
        all_events: timeline,
        live_events: live,
        audio: &[],
        node: me,
        transport: TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
            bpm: 120.0,
            ..Default::default()
        },
        sample_range: 0..BLOCK as i64,
    };
    node.process(&mut ctx);
}

#[test]
fn an_insert_hears_the_notes_of_the_channel_it_names() {
    let (source, _, me) = ids();
    let mut node = insert(source);
    run(&mut node, &[note_on(source, 64)], &[], me);
    assert_eq!(node.held_keys(), &[64], "the timeline's note was not heard");
}

#[test]
fn and_the_live_ones() {
    let (source, _, me) = ids();
    let mut node = insert(source);
    run(&mut node, &[], &[note_on(source, 67)], me);
    assert_eq!(
        node.held_keys(),
        &[67],
        "a key played live did not reach the insert"
    );
}

#[test]
fn and_not_anybody_elses() {
    let (source, other, me) = ids();
    let mut node = insert(source);
    run(
        &mut node,
        &[note_on(other, 60), note_on(source, 64), note_on(me, 72)],
        &[],
        me,
    );
    assert_eq!(
        node.held_keys(),
        &[64],
        "the insert played another channel's part"
    );
}

#[test]
fn a_note_off_lets_go_and_the_last_one_held_is_what_it_reports() {
    let (source, _, me) = ids();
    let mut node = insert(source);
    run(
        &mut node,
        &[
            note_on(source, 60),
            note_on(source, 64),
            note_on(source, 67),
        ],
        &[],
        me,
    );
    assert_eq!(node.held_keys(), &[60, 64, 67]);
    assert_eq!(node.last_key(), Some(67), "last-note priority");
    run(&mut node, &[note_off(source, 67)], &[], me);
    assert_eq!(node.last_key(), Some(64), "letting go went back one");
    run(&mut node, &[note_off(source, 60)], &[], me);
    assert_eq!(node.held_keys(), &[64], "the wrong key was released");
}

#[test]
fn a_reset_lets_go_of_every_held_key() {
    let (source, _, me) = ids();
    let mut node = insert(source);
    run(
        &mut node,
        &[note_on(source, 60), note_on(source, 64)],
        &[],
        me,
    );
    assert_eq!(node.held_keys().len(), 2);
    node.reset();
    assert!(
        node.held_keys().is_empty(),
        "a transport stop left a key held, and the tuner would force it forever"
    );
}

#[test]
fn sixteen_keys_held_is_the_most_it_remembers_and_the_seventeenth_does_not_allocate() {
    let (source, _, me) = ids();
    let mut node = insert(source);
    let notes: Vec<TimedEvent> = (40u8..40 + 20).map(|key| note_on(source, key)).collect();
    run(&mut node, &notes, &[], me);
    assert_eq!(
        node.held_keys().len(),
        16,
        "the stack is a fixed array, not a Vec (INVARIANT 1)"
    );
    // The oldest is dropped rather than the newest refused: the newest is the
    // one a person just played, and a tuner that ignored it would look broken.
    assert_eq!(node.last_key(), Some(59));
}

#[test]
fn an_insert_that_names_nobody_hears_nothing() {
    let (source, _, me) = ids();
    let config = EffectConfig::Tune(TuneConfig::new());
    let mut node = EffectNode::new(config);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    run(&mut node, &[note_on(source, 64)], &[], me);
    assert!(node.held_keys().is_empty());
}
