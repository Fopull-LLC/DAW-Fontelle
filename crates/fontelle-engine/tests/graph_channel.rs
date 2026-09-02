//! Swapping the compiled graph under a running stream.
//!
//! The timeline already reaches a live RT thread through `timeline_channel`,
//! which is what makes "what you draw you hear" true. The *graph* could not:
//! `AudioDevice::start_output_stream` took a `CompiledGraph` by value and the
//! callback kept it for the stream's life, so adding a channel — choosing a
//! soundfont from inside the window — meant restarting the audio device.
//!
//! A `triple_buffer` cannot do this job: it needs `T: Clone`, and a
//! `CompiledGraph` is a bag of `Box<dyn AudioNode>`. So this is the other
//! standard shape — an SPSC queue forward and a **return queue back**, because
//! the one thing the RT thread must never do is drop the graph it just stopped
//! using (INVARIANT 1: freeing a `Patch`, its sample buffers and every node in
//! it is exactly the deallocation the guard exists to catch).
//!
//! These tests pin the three properties that matter, in the order they matter:
//! the swap happens, the old graph comes back to the writer's thread to die,
//! and a return queue with no room means **no swap** rather than a free on the
//! audio thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontelle_engine::{
    AudioNode, BufferPool, CompiledGraph, GRAPH_QUEUE_CAPACITY, ParamSet, PrepareContext,
    ProcessContext, ScheduledNode, graph_channel,
};
use fontelle_types::{NodeId, ParamAddress};

/// A node that writes a constant into its output bus, so which graph is live is
/// readable off the audio, and that **counts its own drop**, so where it was
/// freed is checkable rather than asserted.
struct Marker {
    value: f32,
    drops: Arc<AtomicUsize>,
}

impl Drop for Marker {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct NoParams;
impl ParamSet for NoParams {
    fn get(&self, _addr: &ParamAddress) -> Option<f64> {
        None
    }
    fn set(&self, _addr: &ParamAddress, _value: f64) -> bool {
        false
    }
}

impl AudioNode for Marker {
    fn prepare(&mut self, _ctx: &PrepareContext) {}
    fn process(&mut self, ctx: &mut ProcessContext) {
        let value = self.value;
        for buffer in ctx.outputs.iter_mut() {
            buffer.fill(value);
        }
    }
    fn reset(&mut self) {}
    fn params(&self) -> &dyn ParamSet {
        &NoParams
    }
}

fn graph(value: f32, drops: &Arc<AtomicUsize>) -> CompiledGraph {
    CompiledGraph {
        schedule: vec![ScheduledNode {
            id: NodeId::default(),
            node: Box::new(Marker {
                value,
                drops: drops.clone(),
            }),
            input_buffers: Vec::new(),
            output_buffers: vec![0],
        }],
        buffer_pool: BufferPool::with_capacity(1, 16),
    }
}

/// What the live graph writes, as one number.
fn rendered(graph: &mut CompiledGraph) -> f32 {
    graph.process_block(
        &[],
        fontelle_engine::TransportSnapshot {
            state: fontelle_engine::TransportState::Playing,
            position_sample: 0,
            bpm: fontelle_types::DEFAULT_BPM,
        },
        0..16,
    );
    graph.buffer_pool.buffer_mut(0)[0]
}

#[test]
fn the_source_starts_on_the_graph_it_was_opened_with() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (_publisher, mut source) = graph_channel(graph(1.0, &drops));

    assert_eq!(rendered(source.current()), 1.0);
    assert!(
        !source.take_update(),
        "nothing has been published, so there is nothing to take"
    );
}

#[test]
fn a_published_graph_reaches_the_rt_side_on_the_next_take() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut publisher, mut source) = graph_channel(graph(1.0, &drops));

    publisher.publish(graph(2.0, &drops));
    // Not until it is taken: the swap is the RT thread's own move, made at a
    // block boundary it chooses.
    assert_eq!(rendered(source.current()), 1.0);

    assert!(source.take_update());
    assert_eq!(rendered(source.current()), 2.0);
    assert!(!source.take_update(), "one publish, one swap");
}

#[test]
fn one_take_lands_on_the_newest_rather_than_walking_a_queue() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut publisher, mut source) = graph_channel(graph(1.0, &drops));

    // A burst with the reader asleep. When it wakes it must arrive at the
    // newest in **one** take — a graph swap per queued entry would mean the
    // audio thread walking a backlog of structural changes, one block each.
    for value in 2..=(GRAPH_QUEUE_CAPACITY as i32 + 1) {
        publisher.publish(graph(value as f32, &drops));
    }
    assert!(source.take_update());
    assert_eq!(
        rendered(source.current()),
        (GRAPH_QUEUE_CAPACITY as i32 + 1) as f32,
        "the RT thread must land on the newest graph, not work through a queue"
    );
    assert!(!source.take_update(), "and there is nothing left behind it");
}

#[test]
fn the_replaced_graph_is_freed_by_the_publisher_and_never_by_the_rt_side() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut publisher, mut source) = graph_channel(graph(1.0, &drops));

    publisher.publish(graph(2.0, &drops));
    assert!(source.take_update());
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "the graph the RT thread stopped using must still be alive — freeing it \
         there is the INVARIANT 1 violation this channel exists to prevent"
    );

    let reclaimed = publisher.reclaim();
    assert_eq!(reclaimed, 1, "the old graph comes back to be freed here");
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn the_publisher_always_makes_room_before_it_takes_any() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut publisher, mut source) = graph_channel(graph(0.0, &drops));

    // The state the RT side must never be cornered into is "a graph waiting to
    // be taken, and nowhere to put the one it replaces". It cannot arise while
    // the two queues are the same size and every publish empties the return
    // queue before it fills the forward one — so drive both hard and check the
    // reader is never blocked and never behind.
    let mut expected = 0.0;
    for round in 1..=20 {
        let burst = round % (GRAPH_QUEUE_CAPACITY + 2) + 1;
        for _ in 0..burst {
            expected += 1.0;
            publisher.publish(graph(expected, &drops));
        }
        while source.take_update() {}
        publisher.pump();
        while source.take_update() {}
        assert_eq!(
            rendered(source.current()),
            expected,
            "round {round} left the audio thread behind"
        );
    }
    // And every graph that ever went live was freed on this side. One is still
    // playing, and the publisher holds nothing.
    publisher.reclaim();
    assert_eq!(drops.load(Ordering::SeqCst), expected as usize);
}

#[test]
fn publishing_while_the_queue_is_backed_up_keeps_the_newest_and_lands_it_later() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut publisher, mut source) = graph_channel(graph(0.0, &drops));

    // More publishes than the forward queue can hold, with the RT side asleep.
    for value in 1..=(GRAPH_QUEUE_CAPACITY as i32 + 4) {
        publisher.publish(graph(value as f32, &drops));
    }
    // The reader wakes up and takes whatever is there — then the publisher
    // pumps the one it was holding back.
    while source.take_update() {}
    publisher.pump();
    while source.take_update() {}

    assert_eq!(
        rendered(source.current()),
        (GRAPH_QUEUE_CAPACITY as i32 + 4) as f32,
        "a publish must never be silently lost: the newest one always arrives"
    );
}
